// Invoked only in netns.sh's private network/mount/PID namespaces.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
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
      // Startup changes TUN routes; a pooled pre-start connection can then
      // fail instead of delivering the response selected by this fixture.
      const headers = { Connection: 'close' }
      writeFileSync('/tmp/subscription-requested', '')
      const counter = '/tmp/subscription-request-count'
      writeFileSync(counter, String((existsSync(counter) ? Number(readFileSync(counter, 'utf8')) : 0) + 1))
      if (new URL(request.url).pathname === '/empty' && !existsSync('/tmp/subscription-empty-restored')) {
        return new Response('proxies: []\n', { headers })
      }
      if (!existsSync('/tmp/subscription-online')) return new Response('temporarily unavailable', { status: 503, headers })
      if (new URL(request.url).pathname === '/sub') return new Response(null, {
        status: 302, headers: { ...headers, Location: 'http://bootstrap.test:8080/nodes' },
      })
      return new Response('proxies:\n  - {name: "日本 subscription-node", type: trojan, server: 192.0.2.1, port: 9, password: fixture, sni: example.com}\n', { headers })
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
  async function api(path, body, method = body === undefined ? 'GET' : 'POST') {
    const response = await fetch(`http://127.0.0.1:6161/api/${path}`, {
      method,
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
      const nodes = [manual,
        { ...manual, tag: 'detour-leaf', detour: 'detour-parent' },
        { ...manual, tag: 'detour-parent', detour: 'missing-upstream' },
      ].map(node => JSON.stringify(node))
      const configPath = `/tmp/bootstrap-${mode}.yaml`
      writeFileSync(configPath, Bun.YAML.stringify({ port: 6161, route_mode: mode, node_select: 'fastest_jp', subs: ['http://bootstrap.test:8080/sub'], nodes,
        custom_rules: [JSON.stringify({ source_ip_cidr: '192.168.8.194/32', outbound: 'detour-leaf' })],
      }))
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
      const profileId = createHash('sha256').update(configPath).digest('hex')
      const runtime = JSON.parse(readFileSync(`/tmp/miao-sing-box/profiles/${profileId}/config.json`, 'utf8'))
      assert(!runtime.outbounds.some(node => ['detour-leaf', 'detour-parent'].includes(node.tag)))
      for (const section of ['route', 'dns']) {
        assert(runtime[section].rules.some(rule => rule.source_ip_cidr === '192.168.8.194/32' && rule.action === 'reject'))
      }
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
      assert.deepEqual(saved.nodes, nodes, 'recovery must preserve manual inputs including excluded detours')
      console.log(`PASS ${mode} cold boot isolates broken detours and recovers subscriptions with an unreachable manual node`)
      await stop(app)
      app = undefined
    }

    const configPath = '/tmp/bootstrap-empty.yaml'
    writeFileSync(configPath, Bun.YAML.stringify({ port: 6161, subs: ['http://bootstrap.test:8080/empty'] }))
    const logFd = openSync('/tmp/bootstrap-empty.log', 'w')
    try {
      app = Bun.spawn(['/run/miao-rust', '--config', configPath], { stdout: logFd, stderr: logFd })
    } finally { closeSync(logFd) }
    await until(async () => {
      try {
        const status = await api('status')
        return !status.ready && !status.initializing && status.subscription_refresh.phase === 'waiting'
          && status.subscription_refresh.outcome === 'empty' && status.subscription_refresh.retry_in_secs > 1700
      } catch { return false }
    }, 'successful empty cold boot did not enter the 30 minute wait', 15000)
    const requests = readFileSync('/tmp/subscription-request-count', 'utf8')
    await Bun.sleep(2000)
    assert.equal(readFileSync('/tmp/subscription-request-count', 'utf8'), requests, 'empty response must not trigger fast retries')
    writeFileSync('/tmp/subscription-empty-restored', '')
    await api('subs/refresh', {})
    assert.equal((await api('status')).ready, true, 'fresh nodes must activate without testing remote connectivity')
    console.log('PASS successful empty cold boot waits slowly and a foreground refresh activates recovered nodes')

    // Accept an empty deletion after a successful foreground generation, then
    // exercise the actual Start endpoint. It must begin a new recovery instead
    // of starting retired bytes or inheriting the previous "success" marker.
    rmSync('/tmp/subscription-empty-restored')
    const added = 'http://bootstrap.test:8080/sub'
    await api('subs', { url: added })
    const withBothSources = await api('status')
    assert.equal(withBothSources.ready, true)
    assert.equal(withBothSources.subscription_refresh.report.successful_sources, 2)
    await api('subs', { url: added }, 'DELETE')
    assert.equal((await api('status')).ready, false)
    assert.deepEqual(Bun.YAML.parse(readFileSync(configPath, 'utf8')).subs, ['http://bootstrap.test:8080/empty'])
    const profileId = createHash('sha256').update(configPath).digest('hex')
    assert(!existsSync(`/tmp/miao-sing-box/profiles/${profileId}/config.json`), 'deletion must retire the old active config')
    const started = await fetch('http://127.0.0.1:6161/api/service/start', {
      method: 'POST', signal: AbortSignal.timeout(35000),
    })
    const startResult = await started.json()
    assert.equal(started.status, 400, JSON.stringify(startResult))
    assert.equal(startResult.success, false)
    assert.match(startResult.message, /30 分钟/)
    await until(async () => {
      const status = await api('status')
      return !status.ready && status.subscription_refresh.phase === 'waiting'
        && status.subscription_refresh.retry_in_secs > 1700
    }, 'Start after empty deletion did not retain its new recovery task')
    const afterStart = readFileSync('/tmp/subscription-request-count', 'utf8')
    await Bun.sleep(2000)
    assert.equal(readFileSync('/tmp/subscription-request-count', 'utf8'), afterStart, 'new recovery must keep its slow cadence')
    assert.equal((await api('status')).subscription_refresh.phase, 'waiting')
    writeFileSync('/tmp/subscription-empty-restored', '')
    await api('subs/refresh', {})
    assert.equal((await api('status')).ready, true)
    console.log('PASS empty deletion retires the old runtime and a new Start waits for nodes until foreground recovery')
    await stop(app)
    app = undefined
  } catch (error) {
    for (const mode of ['rule', 'global', 'empty']) {
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
