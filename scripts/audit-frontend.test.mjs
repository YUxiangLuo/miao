import { test, expect } from 'bun:test'
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const shellTest = test.skipIf(process.platform === 'win32')
const audit = new URL('./audit-frontend.sh', import.meta.url).pathname

function runAudit(failures, message) {
  const dir = mkdtempSync(join(tmpdir(), 'miao-audit-'))
  try {
    writeFileSync(join(dir, 'bun'), `#!/bin/sh
count=$(cat "$AUDIT_TEST_DIR/count" 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > "$AUDIT_TEST_DIR/count"
if [ "$count" -le "$AUDIT_TEST_FAILURES" ]; then
  echo "$AUDIT_TEST_MESSAGE" >&2
  exit 1
fi
echo 'No vulnerabilities found'
`, { mode: 0o700 })
    writeFileSync(join(dir, 'sleep'), '#!/bin/sh\nexit 0\n', { mode: 0o700 })
    const result = spawnSync('bash', [audit], {
      encoding: 'utf8',
      env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, AUDIT_TEST_DIR: dir, AUDIT_TEST_FAILURES: String(failures), AUDIT_TEST_MESSAGE: message },
    })
    return { ...result, attempts: Number(readFileSync(join(dir, 'count'), 'utf8')) }
  } finally { rmSync(dir, { recursive: true, force: true }) }
}

const unavailable = 'error: POST https://registry.npmjs.org/-/npm/v1/security/advisories/bulk - 503'

shellTest('audit recovers from a temporary registry outage', () => {
  const result = runAudit(2, unavailable)
  expect(result.status).toBe(0)
  expect(result.attempts).toBe(3)
})

shellTest('audit fails after bounded retries if the registry stays unavailable', () => {
  const result = runAudit(5, unavailable)
  expect(result.status).toBe(1)
  expect(result.attempts).toBe(3)
})

for (const message of ['1 high vulnerability found', 'error: unknown audit failure']) {
  shellTest(`audit fails immediately for ${message}`, () => {
    const result = runAudit(1, message)
    expect(result.status).toBe(1)
    expect(result.attempts).toBe(1)
  })
}

shellTest('successful audit is not retried', () => {
  const result = runAudit(0, '')
  expect(result.status).toBe(0)
  expect(result.attempts).toBe(1)
})
