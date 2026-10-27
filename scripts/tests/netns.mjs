// Invoked only by netns.sh after namespace isolation and fixture setup.
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readlinkSync, writeFileSync } from 'node:fs'
import { createSocket } from 'node:dgram'
import { createConnection } from 'node:net'
import { once } from 'node:events'

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
  const customStart = mode === 'global' ? 2 : 5
  assert.deepEqual(routeRules.slice(customStart, customStart + 4), [
    { domain_suffix: 'direct.example', outbound: 'direct' },
    { domain_suffix: 'pinned.example', outbound: 'smoke-node' },
    { domain_suffix: 'tcp.example', network: 'tcp', outbound: 'smoke-node' },
    { domain_suffix: 'dormant.example', action: 'reject' },
  ])
  assert.equal(routeRules.length, mode === 'global' ? 6 : 9)
  const dnsRules = config.dns.rules
  const customDnsStart = mode === 'global' ? 0 : 1
  assert.deepEqual(dnsRules.slice(customDnsStart, customDnsStart + 3), [
    { domain_suffix: 'direct.example', action: 'route', server: 'local' },
    { domain_suffix: 'pinned.example', action: 'route', server: 'custom-node-dns:smoke-node' },
    { domain_suffix: 'dormant.example', action: 'reject' },
  ])
  assert.equal(dnsRules.length, mode === 'global' ? 3 : 4)
  assert(config.dns.servers.some(server => server.tag === 'custom-node-dns:smoke-node' && server.detour === 'smoke-node'))
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

async function reloadWithDisconnectedClient(mode, expectedPid) {
  const body = JSON.stringify({ route_mode: mode })
  const socket = createConnection({ host: '127.0.0.1', port: 6161 })
  try {
    await once(socket, 'connect')
    socket.write(`POST /api/route-mode HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`)
    const deadline = Date.now() + 5000
    let phase
    while (Date.now() < deadline) {
      phase = (await request('status')).phase
      if (phase === 'reloading') break
      await Bun.sleep(5)
    }
    assert.equal(phase, 'reloading', 'disconnect while readiness and commit are pending')
    socket.destroy()
    const current = await waitReady()
    assert.equal(current.pid, expectedPid)
    // A no-op request waits for the first transaction to finish committing.
    const repeat = await request('route-mode', { route_mode: mode })
    assert.equal(repeat.changed, false, 'disconnected request committed its configuration')
    assert.equal(repeat.runtime_updated, false)
    assert.equal((await request('status')).route_mode, mode)
    await checkRules(mode)
    console.log(`PASS disconnected ${mode} reload completes and keeps kernel PID`)
  } finally {
    socket.destroy()
  }
}

function serviceAlive() {
  try {
    // Bash may not have reaped its background child while running this script.
    return readFileSync(`/proc/${servicePid}/stat`, 'utf8').split(') ')[1][0] !== 'Z'
  } catch (error) {
    if (error.code === 'ENOENT') return false
    throw error
  }
}

async function shutdownWithDisconnectedVps() {
  const body = JSON.stringify({ ip: '203.0.113.10', password: 'isolated-test-password' })
  const socket = createConnection({ host: '127.0.0.1', port: 6161 })
  try {
    await once(socket, 'connect')
    socket.write(`POST /api/vps/deploy HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`)
    const provisionDeadline = Date.now() + 5000
    while (!existsSync('/tmp/vps-provisioned') && Date.now() < provisionDeadline) await Bun.sleep(10)
    assert(existsSync('/tmp/vps-provisioned'), 'VPS request reached remote work outside config_update')
    socket.destroy()
    await Bun.sleep(50)
    process.kill(servicePid, 'SIGTERM')
    // During draining the listener can still answer, but writes must be rejected.
    let response
    const shutdownDeadline = Date.now() + 3000
    do {
      response = await fetch('http://127.0.0.1:6161/api/mcp', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled: false }), signal: AbortSignal.timeout(1000),
      })
      if (response.status === 503) break
      await Bun.sleep(10)
    } while (Date.now() < shutdownDeadline)
    assert.equal(response.status, 503, 'shutdown closes admission to new writes')
    assert.equal((await response.json()).message, 'Service is shutting down')
    await Bun.sleep(100)
    assert(serviceAlive(), 'shutdown must wait for the disconnected VPS request')
    const before = Bun.YAML.parse(readFileSync('/tmp/miao-smoke.yaml', 'utf8'))
    assert.equal(before.nodes.length, 1, 'VPS local commit is still pending')
  } finally {
    socket.destroy()
    writeFileSync('/tmp/finish-vps', '')
  }
  const deadline = Date.now() + 5000
  while (serviceAlive() && Date.now() < deadline) await Bun.sleep(20)
  assert(!serviceAlive(), 'service exits after the accepted write finishes')
  const saved = Bun.YAML.parse(readFileSync('/tmp/miao-smoke.yaml', 'utf8'))
  assert(saved.nodes.some(node => JSON.parse(node).server === '203.0.113.10'), 'VPS node committed before process exit')
  assert.equal(existsSync('/sys/class/net/sing-tun'), false, 'shutdown removes TUN')
  console.log('PASS SIGTERM drains disconnected VPS write, rejects later writes and removes TUN')
}

async function shutdownWithIncompleteBody() {
  const socket = createConnection({ host: '127.0.0.1', port: 6161 })
  try {
    await once(socket, 'connect')
    const receiving = once(socket, 'data', { signal: AbortSignal.timeout(2000) })
    socket.write('POST /api/mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 17\r\nExpect: 100-continue\r\n\r\n')
    const [interim] = await receiving
    assert.equal(interim.toString(), 'HTTP/1.1 100 Continue\r\n\r\n', 'server started receiving the body')
    socket.write('{')
    // The completed VPS request must drain, while this unfinished upload must
    // be cancelled. Keep its client open through the entire process shutdown.
    await shutdownWithDisconnectedVps()
    console.log('PASS incomplete POST does not delay SIGTERM shutdown')
  } finally {
    socket.destroy()
  }
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

for (const mode of ['global', 'rule']) {
  await reloadWithDisconnectedClient(mode, first.pid)
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
await shutdownWithIncompleteBody()
console.log('REAL KERNEL SMOKE PASS')
