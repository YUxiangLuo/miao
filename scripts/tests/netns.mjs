// Invoked only by netns.sh after namespace isolation and fixture setup.
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readlinkSync } from 'node:fs'

assert.equal(process.getuid(), 0, 'run through ./dev.sh netns')
assert.equal(readlinkSync('/proc/self/ns/net'), readlinkSync('/proc/1/ns/net'))
assert.equal(readFileSync('/proc/1/comm', 'utf8').trim(), 'bash', 'expected the isolated worker as PID 1')
assert(existsSync('/run/miao-rust') && existsSync('/tmp/miao-smoke.yaml'), 'missing isolated fixtures')
const servicePid = Number(process.argv[2])
assert(Number.isInteger(servicePid) && servicePid > 1, 'missing isolated service PID')

async function request(path, body) {
  const response = await fetch(`http://127.0.0.1:6161/api/${path}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(12_000),
  })
  const payload = await response.json()
  assert(response.ok && payload.success, `${path}: ${JSON.stringify(payload)}`)
  return payload.data
}

async function waitReady() {
  const deadline = Date.now() + 25_000
  let last
  while (Date.now() < deadline) {
    try {
      process.kill(servicePid, 0)
    } catch {
      throw new Error('Miao exited before the kernel became ready')
    }
    try {
      last = await request('status')
      if (last.ready && !last.initializing) return last
    } catch (error) {
      last = String(error)
    }
    await Bun.sleep(100)
  }
  throw new Error(`Kernel did not become ready: ${JSON.stringify(last)}`)
}

async function checkRules(mode) {
  const files = [...new Bun.Glob('**/config.json').scanSync('/tmp/miao-sing-box')]
  assert.equal(files.length, 1, 'exactly one isolated runtime config')
  const config = await Bun.file(`/tmp/miao-sing-box/${files[0]}`).json()
  const routeRules = config.route.rules
  assert.deepEqual(routeRules.slice(2, 5), [
    { domain_suffix: 'direct.example', outbound: 'direct' },
    { domain_suffix: 'pinned.example', outbound: 'smoke-node' },
    { domain_suffix: 'tcp.example', network: 'tcp', outbound: 'smoke-node' },
  ])
  assert.equal(routeRules.length, mode === 'global' ? 5 : 8)
  const dnsRules = config.dns.rules
  assert.deepEqual(dnsRules.slice(0, 2), [
    { domain_suffix: 'direct.example', action: 'route', server: 'local' },
    { domain_suffix: 'pinned.example', action: 'route', server: 'custom-node-dns-1' },
  ])
  assert.equal(dnsRules.length, mode === 'global' ? 2 : 3)
  assert(config.dns.servers.some(server => server.tag === 'custom-node-dns-1' && server.detour === 'smoke-node'))
  console.log(`PASS ${mode} route/DNS rules, including skipped missing target`)
}

async function stopAndCheck() {
  await request('service/stop', {})
  const stopped = await request('status')
  assert.equal(stopped.running, false)
  assert.equal(stopped.ready, false)
  assert.equal(stopped.phase, 'stopped')
  assert.equal(existsSync('/sys/class/net/sing-tun'), false, 'TUN removed after stop')
}

const first = await waitReady()
assert(existsSync('/sys/class/net/sing-tun'), 'TUN created on startup')
await checkRules('rule')
console.log(`PASS startup ready, kernel PID ${first.pid}`)

for (const mode of ['global', 'rule']) {
  const result = await request('route-mode', { route_mode: mode })
  const current = await waitReady()
  assert.equal(current.pid, first.pid, 'Unix reload preserves kernel PID')
  assert.equal(current.route_mode, mode)
  assert.equal(result.reloaded, true)
  assert.equal(result.restarted, false)
  await checkRules(mode)
  console.log(`PASS reload ${mode}, same kernel PID`)
}

const fallback = await request('node-select', { node_select: 'fastest_jp' })
assert.equal(fallback.requested, 'fastest_jp')
assert.equal(fallback.node_select, 'manual')
await request('node-select', { node_select: 'manual' })
console.log('PASS strategy fallback and manual reset')

await stopAndCheck()
console.log('PASS stop, process stopped and TUN removed')
await request('service/start', {})
const restarted = await waitReady()
assert.notEqual(restarted.pid, first.pid)
assert(existsSync('/sys/class/net/sing-tun'), 'TUN recreated after restart')
await checkRules('rule')
console.log(`PASS restart ready, new kernel PID ${restarted.pid}`)
await stopAndCheck()
console.log('REAL KERNEL SMOKE PASS')
