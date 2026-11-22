import { afterEach, expect, test } from 'bun:test'
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const roots = []
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }) })
const head = 'a'.repeat(40)
const geoipSha = 'b'.repeat(40)
const directSha = 'c'.repeat(40)
const shellTest = test.skipIf(process.platform === 'win32')

function runCiRelease(args, overrides = {}) {
  const root = mkdtempSync(join(tmpdir(), 'harbor-ci-release-test-'))
  roots.push(root)
  mkdirSync(join(root, 'scripts/lib'), { recursive: true })
  mkdirSync(join(root, 'bin'))
  copyFileSync(new URL('../lib/ci-release.sh', import.meta.url), join(root, 'scripts/lib/ci-release.sh'))
  const log = join(root, 'calls')
  const outputFile = join(root, 'github_output')
  const summaryFile = join(root, 'github_summary')
  writeFileSync(log, '')
  writeFileSync(outputFile, '')
  writeFileSync(summaryFile, '')

  const stub = `#!/bin/bash
tool=\${0##*/}
printf '%s %s\\n' "$tool" "$*" >> "$LOG"
case "$tool:$*" in
  'git:ls-remote --tags origin '*) printf '%s' "\${REMOTE_TAG:-}" ;;
  'git:ls-remote --exit-code https://github.com/SagerNet/sing-geoip.git refs/heads/rule-set') printf '%s\\t%s\\n' "${geoipSha}" "refs/heads/rule-set" ;;
  'git:ls-remote --exit-code https://github.com/Loyalsoldier/v2ray-rules-dat.git refs/heads/release') printf '%s\\t%s\\n' "${directSha}" "refs/heads/release" ;;
  'bun:scripts/lib/release-artifacts.mjs version') echo "\${WORKSPACE_VERSION:-1.2.3}" ;;
  'bun:scripts/sing-box/source.mjs go_version') echo '1.27.1' ;;
  'bun:scripts/lib/release-artifacts.mjs assemble '*) exit "\${ASSEMBLE_STATUS:-0}" ;;
  'bun:scripts/lib/release-artifacts.mjs verify '*) exit "\${VERIFY_STATUS:-0}" ;;
  'bun:scripts/lib/release-artifacts.mjs assets') printf '%s\\n' harbor-linux-amd64 harbor-linux-amd64.sha256 harbor-linux-arm64 harbor-linux-arm64.sha256 harbor-windows-amd64-setup.exe harbor-windows-amd64-setup.exe.sha256 ;;
  'bun:scripts/lib/release-artifacts.mjs summary '*) printf '## Summary %s\\n' "$*" ;;
  'gh:api '*)
    [[ \${GH_API_STATUS:-0} == 0 ]] || exit "$GH_API_STATUS"
    if [[ \${EXISTING_ASSETS+x} ]]; then printf '%s\\n%s\\n' "\${RELEASE_STATE:-published}" "$EXISTING_ASSETS"; fi ;;
  'gh:release upload '*) exit "\${UPLOAD_STATUS:-0}" ;;
  'gh:release view '*) echo 'https://github.com/YUxiangLuo/miao/releases/tag/v1.2.3' ;;
  'sha256sum:'*) exit 0 ;;
esac
`
  for (const name of ['git', 'bun', 'gh', 'sha256sum']) {
    writeFileSync(join(root, 'bin', name), stub, { mode: 0o755 })
  }
  const stageDir = join(root, 'dist', 'v1.2.3')
  mkdirSync(stageDir, { recursive: true })

  const result = Bun.spawnSync(['bash', join(root, 'scripts/lib/ci-release.sh'), ...args], {
    env: {
      ...process.env,
      PATH: `${join(root, 'bin')}:/usr/bin:/bin`,
      LOG: log,
      GITHUB_REPOSITORY: 'YUxiangLuo/miao',
      GITHUB_OUTPUT: outputFile,
      GITHUB_STEP_SUMMARY: summaryFile,
      COMMIT: head,
      ...overrides,
    },
    stdout: 'pipe',
    stderr: 'pipe',
  })

  return {
    status: result.exitCode,
    calls: readFileSync(log, 'utf8').trim().split('\n').filter(Boolean),
    outputs: Object.fromEntries(
      readFileSync(outputFile, 'utf8').trim().split('\n').filter(Boolean).map(line => line.split('=')),
    ),
    summary: readFileSync(summaryFile, 'utf8'),
    error: result.stderr.toString(),
    stageDir,
  }
}

shellTest('preflight validates tag/version alignment, pins rules and go version, and rejects invalid inputs', () => {
  const ok = runCiRelease(['preflight'], { EVENT_NAME: 'push', REF_NAME: 'v1.2.3', REMOTE_TAG: `${head}\trefs/tags/v1.2.3` })
  expect(ok.status).toBe(0)
  expect(ok.outputs).toEqual({
    tag: 'v1.2.3',
    version: '1.2.3',
    commit: head,
    draft: 'false',
    go_version: '1.27.1',
    sing_geoip_ref: geoipSha,
    direct_rules_ref: directSha,
  })

  const dispatchDraft = runCiRelease(['preflight'], {
    EVENT_NAME: 'workflow_dispatch',
    REF_NAME: 'master',
    GITHUB_REF_TYPE: 'branch',
    INPUT_DRAFT: 'true',
    INPUT_EXPECTED_VERSION: 'v1.2.3',
  })
  expect(dispatchDraft.status).toBe(0)
  expect(dispatchDraft.outputs.draft).toBe('true')
  expect(dispatchDraft.outputs.tag).toBe('v1.2.3')

  expect(runCiRelease(['preflight'], { EVENT_NAME: 'push', REF_NAME: 'v1.2.4' }).status).toBe(1)
  expect(runCiRelease(['preflight'], { EVENT_NAME: 'workflow_dispatch', INPUT_EXPECTED_VERSION: 'v9.9.9' }).status).toBe(1)
  expect(runCiRelease(['preflight'], { EVENT_NAME: 'workflow_dispatch', INPUT_DRAFT: 'true', EXISTING_ASSETS: '', RELEASE_STATE: 'published' }).status).toBe(1)
  expect(runCiRelease(['preflight'], { EVENT_NAME: 'push', REF_NAME: 'v1.2.3', REMOTE_TAG: `${'d'.repeat(40)}\trefs/tags/v1.2.3` }).status).toBe(1)
})

shellTest('publish creates tag if absent, uploads all 6 Linux + Windows assets and writes step summary', () => {
  const res = runCiRelease(['publish', 'dist/v1.2.3', head, 'v1.2.3', 'true'])
  expect(res.status).toBe(0)
  expect(res.calls).toContain(`git tag -f v1.2.3 ${head}`)
  expect(res.calls).toContain('git push origin refs/tags/v1.2.3')
  const create = res.calls.find(call => call.startsWith('gh release create '))
  expect(create).toContain('--draft')
  expect(create).toContain('--verify-tag')
  expect(create).toContain('dist/v1.2.3/harbor-linux-amd64')
  expect(create).toContain('dist/v1.2.3/harbor-linux-arm64')
  expect(create).toContain('dist/v1.2.3/harbor-windows-amd64-setup.exe')
  expect(create).toContain('dist/v1.2.3/harbor-windows-amd64-setup.exe.sha256')
  expect(res.summary).toContain('## Summary')
})

shellTest('publish promotes existing draft and deletes obsolete assets only after uploads succeed', () => {
  const env = {
    REMOTE_TAG: `${head}\trefs/tags/v1.2.3`,
    RELEASE_STATE: 'draft',
    EXISTING_ASSETS: 'harbor-linux-amd64\nharbor-linux-amd64.sha256\nlegacy-file.tar.gz',
  }
  const ok = runCiRelease(['publish', 'dist/v1.2.3', head, 'v1.2.3', 'false'], env)
  expect(ok.status).toBe(0)
  expect(ok.calls.some(call => call.startsWith('git push '))).toBe(false)
  const uploadIdx = ok.calls.findIndex(call => call.startsWith('gh release upload '))
  const deleteIdx = ok.calls.findIndex(call => call.startsWith('gh release delete-asset '))
  const editIdx = ok.calls.findIndex(call => call.startsWith('gh release edit ') && call.includes('--draft=false'))
  expect(deleteIdx).toBeGreaterThan(uploadIdx)
  expect(editIdx).toBeGreaterThan(uploadIdx)

  const failed = runCiRelease(['publish', 'dist/v1.2.3', head, 'v1.2.3', 'false'], { ...env, UPLOAD_STATUS: '1' })
  expect(failed.status).toBe(1)
  expect(failed.calls.some(call => call.startsWith('gh release delete-asset '))).toBe(false)
  expect(failed.calls.some(call => call.startsWith('gh release edit '))).toBe(false)
})
