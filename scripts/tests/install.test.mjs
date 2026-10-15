import { afterEach, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const roots = []
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }) })
const name = 'miao-rust-linux-amd64'
const binary = 'downloaded fixture binary'
const digest = `sha256:${createHash('sha256').update(binary).digest('hex')}`

function download(assets) {
  const root = mkdtempSync(join(tmpdir(), 'miao-install-test-'))
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
    https://example.com/v1.2.3/miao) cp "$FIXTURE/binary" "$2" ;;
    *) return 1 ;;
  esac
}
download_release_binary miao-rust-linux-amd64 "$FIXTURE" "$FIXTURE/candidate"
`], {
    env: { ...process.env, INSTALL_SCRIPT: fileURLToPath(new URL('../../install.sh', import.meta.url)), FIXTURE: root },
    stdout: 'pipe', stderr: 'pipe',
  })
  return { status: result.exitCode, calls: readFileSync(join(root, 'calls'), 'utf8').trim().split('\n'), root }
}

test('installer downloads a pinned release URL and verifies its GitHub digest without sidecar assets', () => {
  const result = download([{ name, browser_download_url: 'https://example.com/v1.2.3/miao', digest }])
  expect(result.status).toBe(0)
  expect(result.calls).toHaveLength(2)
  expect(result.calls[1]).toBe('https://example.com/v1.2.3/miao')
  expect(readFileSync(join(result.root, 'candidate'), 'utf8')).toBe(binary)
})

test('installer rejects missing, duplicate or invalid digests before downloading a binary', () => {
  const asset = { name, browser_download_url: 'https://example.com/v1.2.3/miao', digest }
  for (const assets of [[], [asset, asset], [{ ...asset, digest: null }], [{ ...asset, digest: 'sha256:bad' }], [{ ...asset, digest: digest.replace('sha256', 'sha512') }]]) {
    const result = download(assets)
    expect(result.status).not.toBe(0)
    expect(result.calls).toHaveLength(1)
  }
})

test('installer refuses a binary whose SHA256 differs from release metadata', () => {
  const result = download([{ name, browser_download_url: 'https://example.com/v1.2.3/miao', digest: `sha256:${'a'.repeat(64)}` }])
  expect(result.status).not.toBe(0)
  expect(result.calls).toHaveLength(2)
})

test('installation and removal help and argument errors are handled before privilege checks', () => {
  const root = mkdtempSync(join(tmpdir(), 'miao-entry-test-'))
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
