// Invoked only by netns.sh after namespace isolation and fixture setup.
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readlinkSync } from 'node:fs'
import { createSocket } from 'node:dgram'

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
  assert.deepEqual(routeRules.slice(2, 6), [
    { domain_suffix: 'direct.example', outbound: 'direct' },
    { domain_suffix: 'pinned.example', outbound: 'smoke-node' },
    { domain_suffix: 'tcp.example', network: 'tcp', outbound: 'smoke-node' },
    { domain_suffix: 'dormant.example', action: 'reject' },
  ])
  assert.equal(routeRules.length, mode === 'global' ? 6 : 9)
  const dnsRules = config.dns.rules
  assert.deepEqual(dnsRules.slice(0, 3), [
    { domain_suffix: 'direct.example', action: 'route', server: 'local' },
    { domain_suffix: 'pinned.example', action: 'route', server: 'custom-node-dns-1' },
    { domain_suffix: 'dormant.example', action: 'reject' },
  ])
  assert.equal(dnsRules.length, mode === 'global' ? 3 : 4)
  assert(config.dns.servers.some(server => server.tag === 'custom-node-dns-1' && server.detour === 'smoke-node'))
  const rules = await request('rules')
  assert(rules.some(rule => rule.target === 'gone-node' && rule.skipped))
  const status = await request('status')
  assert(status.warnings.some(warning => warning.code === 'custom_rule_outbound_missing' && warning.message.includes('已阻断')))
  await checkBlockedDns()
  console.log(`PASS ${mode} route/DNS rules, including blocked missing target`)
}

async function checkBlockedDns() {
  // Send through TUN DNS hijacking. The missing-node policy must answer REFUSED
  // locally instead of attempting the unreachable default upstream resolver.
  const query = Buffer.from('4d490100000100000000000007646f726d616e74076578616d706c650000010001', 'hex')
  const socket = createSocket('udp4')
  try {
    const reply = await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Missing-node DNS was not rejected')), 3000)
      socket.once('error', error => { clearTimeout(timeout); reject(error) })
      socket.once('message', message => { clearTimeout(timeout); resolve(message) })
      socket.send(query, 53, '8.8.8.8')
    })
    assert.equal(reply.readUInt16BE(0), 0x4d49)
    assert.equal(reply[3] & 0x0f, 5, 'missing-node DNS returns REFUSED')
  } finally {
    socket.close()
  }
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
