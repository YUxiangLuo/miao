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
  'git:rev-parse --verify '*) exit 1 ;;
  'git:remote get-url --push --all origin') printf '%s\\n' "\${PUSH_URL:-git@github.com:example/miao.git}" ;;
  'git:ls-remote '*) printf '%s' "\${REMOTE_TAG:-}" ;;
  'bun:scripts/lib/release-artifacts.mjs version') echo 1.2.3 ;;
  'bun:scripts/lib/release-artifacts.mjs version '*) echo 'updated manifest' > Cargo.toml ;;
  'cargo:check '*--offline*) echo 'updated lock' > Cargo.lock; exit "\${CARGO_STATUS:-0}" ;;
  'bun:scripts/lib/release-artifacts.mjs verify '*) exit "\${VERIFY_STATUS:-0}" ;;
  'rustup:target list --installed') printf '%s\\n' x86_64-unknown-linux-musl aarch64-unknown-linux-musl ;;
  'gh:api '*)
    [[ \${GH_API_STATUS:-0} == 0 ]] || exit "$GH_API_STATUS"
    if [[ \${EXISTING_ASSETS+x} ]]; then printf '%s\\n%s\\n' "\${RELEASE_STATE:-published}" "$EXISTING_ASSETS"; fi ;;
  'gh:release upload '*) exit "\${UPLOAD_STATUS:-0}" ;;
esac
`
  for (const name of ['git', 'bun', 'cargo', 'gh', 'rustup', 'zig', 'cargo-zigbuild', 'go', 'curl', 'shellcheck', 'jq']) {
    writeFileSync(join(root, 'bin', name), stub, { mode: 0o755 })
  }
  const result = Bun.spawnSync(['bash', join(root, 'dev.sh'), ...args], {
    env: { ...process.env, PATH: `${join(root, 'bin')}:/usr/bin:/bin`, LOG: log, HEAD: head, ...overrides },
    stdout: 'pipe', stderr: 'pipe',
  })
  return {
    status: result.exitCode, calls: readFileSync(log, 'utf8').trim().split('\n'), error: result.stderr.toString(),
    manifest: readFileSync(join(root, 'Cargo.toml'), 'utf8'), lock: readFileSync(join(root, 'Cargo.lock'), 'utf8'), root,
  }
}

test('build rejects ambiguous targets and core checks do not need Bun or resources', () => {
  expect(run(['build', 'amd64', 'arm64']).status).toBe(1)
  expect(run(['version', 'v1.2.3;bad']).status).toBe(1)
  const result = run(['check', 'core'])
  expect(result.status).toBe(0)
  expect(result.calls.every(call => call.startsWith('cargo '))).toBe(true)
  expect(result.calls.filter(call => call.includes('--no-default-features'))).toHaveLength(2)
})

test('frontend and script checks run independently', () => {
  const frontend = run(['check', 'frontend'])
  expect(frontend.status).toBe(0)
  expect(frontend.calls.every(call => call.startsWith('bun ') && call.includes('frontend-rsbuild'))).toBe(true)
  const scripts = run(['check', 'scripts'])
  expect(scripts.status).toBe(0)
  expect(scripts.calls.some(call => call.startsWith('shellcheck '))).toBe(true)
  expect(scripts.calls).toContain('bun test scripts/tests')
  expect(scripts.calls.some(call => call.includes('frontend-rsbuild') || call.startsWith('cargo '))).toBe(false)
})

test('kernel-only builds accept the default host target', () => {
  const defaultTarget = run(['kernel']).calls.find(call => call.startsWith('kernel '))
  const result = run(['kernel', '--kernel-only'])
  expect(result.status).toBe(0)
  expect(result.calls).toContain(`${defaultTarget} --kernel-only`)
  expect(run(['kernel', 'arm64', '--kernel-only']).calls).toContain('kernel arm64 --kernel-only')
})

test('release preparation builds both musl targets with one kernel build and no remote writes', () => {
  const result = run(['release'], { CARGO_TARGET_DIR: '/unused-review-target' })
  expect(result.status).toBe(0)
  expect(result.calls.filter(call => call === 'kernel all')).toHaveLength(1)
  expect(result.calls.filter(call => call.startsWith('cargo zigbuild'))).toHaveLength(2)
  expect(result.calls.filter(call => call.startsWith('cargo zigbuild')).every(call => call.includes(`--target-dir ${result.root}/target`))).toBe(true)
  expect(result.calls.some(call => call.includes('mjs stage '))).toBe(true)
  expect(result.calls.some(call => /^(gh |git (push|tag) )/.test(call))).toBe(false)
})

test('publishing rejects dirty trees, unverified builds and mismatched remote tags before writes', () => {
  for (const env of [{ DIRTY: ' M changed' }, { GIT_STATUS: '1' }, { VERIFY_STATUS: '1' }, { GH_API_STATUS: '1' }, { REMOTE_TAG: `${'b'.repeat(40)}\trefs/tags/v1.2.3` }, { PUSH_URL: 'git@github.com:a/one.git\ngit@github.com:b/two.git' }]) {
    const result = run(['release', '--publish'], env)
    expect(result.status).toBe(1)
    expect(result.calls.some(call => /^(git (push|tag) |gh release (create|upload|delete-asset) )/.test(call))).toBe(false)
  }
})

test('version updates restore both original files when Cargo fails', () => {
  const failed = run(['version', 'v1.2.4'], { CARGO_STATUS: '1' })
  expect(failed.status).toBe(1)
  expect(failed.manifest).toBe('original manifest\n')
  expect(failed.lock).toBe('original lock\n')
  const success = run(['version', 'v1.2.4'])
  expect(success.status).toBe(0)
  expect(success.manifest).toBe('updated manifest\n')
  expect(success.lock).toBe('updated lock\n')
})

test('tag checks, push and GitHub uploads all use the origin push URL', () => {
  const pushUrl = 'https://github.com/release-owner/miao.git'
  const result = run(['release', '--publish'], { PUSH_URL: pushUrl })
  expect(result.status).toBe(0)
  expect(result.calls.some(call => call.startsWith(`git ls-remote --tags ${pushUrl} `))).toBe(true)
  expect(result.calls).toContain(`git push ${pushUrl} refs/tags/v1.2.3`)
  expect(result.calls.find(call => call.startsWith('gh release create '))).toContain('--repo github.com/release-owner/miao')
})

test('draft releases are published only after uploads, while --draft cannot overwrite a public release', () => {
  const env = { EXISTING_ASSETS: '', RELEASE_STATE: 'draft' }
  const publish = run(['release', '--publish'], env)
  expect(publish.status).toBe(0)
  const upload = publish.calls.findIndex(call => call.startsWith('gh release upload '))
  const edit = publish.calls.findIndex(call => call.startsWith('gh release edit ') && call.includes('--draft=false'))
  expect(edit).toBeGreaterThan(upload)
  for (const result of [run(['release', '--publish', '--draft'], env), run(['release', '--publish'], { ...env, UPLOAD_STATUS: '1' })]) {
    expect(result.calls.some(call => call.startsWith('gh release edit '))).toBe(false)
  }
  const rejected = run(['release', '--publish', '--draft'], { EXISTING_ASSETS: '', RELEASE_STATE: 'published' })
  expect(rejected.status).toBe(1)
  expect(rejected.calls.some(call => /^(git (push|tag) |gh release (create|upload|edit) )/.test(call))).toBe(false)
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
