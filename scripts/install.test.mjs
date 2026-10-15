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
    env: { ...process.env, INSTALL_SCRIPT: fileURLToPath(new URL('../install.sh', import.meta.url)), FIXTURE: root },
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
