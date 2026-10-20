// Invoked only in netns.sh's private network/mount/PID namespaces.
import assert from 'node:assert/strict'
import { createSocket } from 'node:dgram'
import { closeSync, existsSync, openSync, readFileSync, readlinkSync, writeFileSync, rmSync } from 'node:fs'

assert.equal(process.getuid(), 0)
assert.equal(readFileSync('/proc/1/comm', 'utf8').trim(), 'bash')
assert(existsSync('/run/miao-rust') && existsSync('/tmp/miao-smoke.yaml'))

if (process.argv[2] === '--upstream') {
  // A simulated Internet host and DNS resolver in a second network namespace.
  // No public network is contacted. Answers include both A and empty AAAA.
  const dns = createSocket('udp4')
  dns.on('message', (query, peer) => {
    let end = 12
    while (end < query.length && query[end] !== 0) end += query[end] + 1
    end += 5
    if (end > query.length) return
    const isA = query.readUInt16BE(end - 4) === 1
    const header = Buffer.from(query.subarray(0, 12))
    header.writeUInt16BE(0x8180, 2)
    header.writeUInt16BE(isA ? 1 : 0, 6)
    header.writeUInt32BE(0, 8)
    const answer = isA ? Buffer.from('c00c00010001000000010004c633640a', 'hex') : Buffer.alloc(0)
    dns.send(Buffer.concat([header, query.subarray(12, end), answer]), peer.port, peer.address)
  })
  dns.bind(53, '223.5.5.5')
  Bun.serve({
    hostname: '198.51.100.10', port: 8080,
    fetch(request) {
      writeFileSync('/tmp/subscription-requested', '')
      if (!existsSync('/tmp/subscription-online')) return new Response('temporarily unavailable', { status: 503 })
      if (new URL(request.url).pathname === '/sub') return Response.redirect('http://bootstrap.test:8080/nodes', 302)
      return new Response('proxies:\n  - {name: "日本 subscription-node", type: trojan, server: 192.0.2.1, port: 9, password: fixture, sni: example.com}\n')
    },
  })
  writeFileSync('/tmp/subscription-upstream-ready', '')
} else {
  assert.equal(readlinkSync('/proc/self/ns/net'), readlinkSync('/proc/1/ns/net'))
  function command(...args) {
    const result = Bun.spawnSync(args, { stdout: 'pipe', stderr: 'pipe' })
    assert.equal(result.exitCode, 0, `${args.join(' ')}: ${result.stderr}`)
  }
  async function until(check, message, ms = 10000) {
    const deadline = Date.now() + ms
    while (Date.now() < deadline) {
      if (await check()) return
      await Bun.sleep(50)
    }
    throw new Error(message)
  }
  async function api(path, body) {
    const response = await fetch(`http://127.0.0.1:6161/api/${path}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(35000),
    })
    const payload = await response.json()
    assert(response.ok && payload.success, `${path}: ${JSON.stringify(payload)}`)
    return payload.data
  }
  async function stop(child) {
    if (!child) return
    child.kill('SIGTERM')
    const exited = await Promise.race([child.exited.then(() => true), Bun.sleep(5000).then(() => false)])
    if (!exited) { child.kill('SIGKILL'); await child.exited }
    assert(exited, 'fixture did not stop gracefully')
  }

  command('ip', 'link', 'del', 'test-uplink')
  command('ip', 'netns', 'add', 'miao-upstream')
  let upstream
  let app
  try {
    command('ip', 'link', 'add', 'boot-uplink', 'type', 'veth', 'peer', 'name', 'boot-peer')
    command('ip', 'link', 'set', 'boot-peer', 'netns', 'miao-upstream')
    command('ip', 'address', 'add', '192.0.2.2/24', 'dev', 'boot-uplink')
    command('ip', 'link', 'set', 'boot-uplink', 'up')
    command('ip', 'route', 'add', 'default', 'via', '192.0.2.1')
    for (const args of [
      ['address', 'add', '192.0.2.1/24', 'dev', 'boot-peer'],
      ['address', 'add', '198.51.100.10/32', 'dev', 'lo'],
      ['address', 'add', '223.5.5.5/32', 'dev', 'lo'],
      ['link', 'set', 'boot-peer', 'up'], ['link', 'set', 'lo', 'up'],
    ]) command('ip', '-n', 'miao-upstream', ...args)
    writeFileSync('/tmp/bootstrap-resolv.conf', 'nameserver 223.5.5.5\noptions timeout:1 attempts:1\n')
    command('mount', '--bind', '/tmp/bootstrap-resolv.conf', '/etc/resolv.conf')
    upstream = Bun.spawn(['ip', 'netns', 'exec', 'miao-upstream', '/run/bun', '/run/netns-subscription.mjs', '--upstream'], { stdout: 'inherit', stderr: 'inherit' })
    await until(() => existsSync('/tmp/subscription-upstream-ready'), 'fixture did not start')

    for (const mode of ['rule', 'global']) {
      rmSync('/tmp/subscription-online', { force: true })
      rmSync('/tmp/subscription-requested', { force: true })
      const manual = { type: 'trojan', tag: 'dead-manual', server: '192.0.2.1', server_port: 9, password: 'fixture', tls: { enabled: true, server_name: 'example.com' } }
      const configPath = `/tmp/bootstrap-${mode}.yaml`
      writeFileSync(configPath, Bun.YAML.stringify({ port: 6161, route_mode: mode, node_select: 'fastest_jp', subs: ['http://bootstrap.test:8080/sub'], nodes: [JSON.stringify(manual)] }))
      const logFd = openSync(`/tmp/bootstrap-${mode}.log`, 'w')
      try {
        app = Bun.spawn(['/run/miao-rust', '--config', configPath], { stdout: logFd, stderr: logFd })
      } finally {
        closeSync(logFd)
      }
      await until(async () => {
        try { const status = await api('status'); return status.ready && !status.initializing } catch { return false }
      }, 'manual-only startup did not become ready', 15000)
      assert.equal((await api('status')).route_mode, mode)
      // This request must reach the simulated server despite the dead default.
      // Its initial 503 also exercises a real failure followed by recovery.
      await until(() => existsSync('/tmp/subscription-requested'), 'dead manual node prevented subscription DNS/HTTP from reaching its server')
      writeFileSync('/tmp/subscription-online', '')
      await until(async () => {
        const status = await api('status')
        return status.subscription_refresh.report?.successful_sources === 1 && status.node_select === 'fastest_jp'
      }, 'background subscription recovery did not complete', 15000)
      await api('subs/refresh', {})
      const status = await api('status')
      assert.equal(status.subscription_refresh.report.successful_sources, 1)
      assert.equal(status.subscription_refresh.report.fresh_nodes, 1)
      assert.equal(status.node_select, 'fastest_jp')
      const saved = Bun.YAML.parse(readFileSync(configPath, 'utf8'))
      assert.deepEqual(saved.nodes, [JSON.stringify(manual)], 'recovery must preserve the manual node')
      console.log(`PASS ${mode} cold boot recovers subscription DNS/HTTP with an unreachable manual node`)
      await stop(app)
      app = undefined
    }
  } catch (error) {
    for (const mode of ['rule', 'global']) {
      const log = `/tmp/bootstrap-${mode}.log`
      if (existsSync(log)) console.error(readFileSync(log, 'utf8').split('\n').slice(-45).join('\n'))
    }
    throw error
  } finally {
    await stop(app)
    await stop(upstream)
    command('ip', 'netns', 'del', 'miao-upstream')
  }
}
