// Two LAN clients, two real VLESS servers and controlled Internet/DNS endpoints.
// Only netns.sh may invoke this: all addresses, certificates and processes are
// confined to its private network/mount/PID namespaces. No public traffic.
import assert from 'node:assert/strict'
import { createSocket } from 'node:dgram'
import { closeSync, existsSync, openSync, readFileSync, readlinkSync, writeFileSync } from 'node:fs'

assert.equal(process.getuid(), 0)
assert.equal(readFileSync('/proc/1/comm', 'utf8').trim(), 'bash')
assert(existsSync('/run/miao-rust') && existsSync('/tmp/miao-smoke.yaml'))

function dnsAnswer(query, address) {
  let end = 12
  while (end < query.length && query[end] !== 0) end += query[end] + 1
  end += 5
  assert(end <= query.length, 'valid fixture DNS query')
  const isA = query.readUInt16BE(end - 4) === 1
  const header = Buffer.from(query.subarray(0, 12))
  header.writeUInt16BE(0x8180, 2)
  header.writeUInt16BE(isA ? 1 : 0, 6)
  header.writeUInt32BE(0, 8)
  const answer = isA ? Buffer.concat([
    Buffer.from('c00c000100010000003c0004', 'hex'), Buffer.from(address.split('.').map(Number)),
  ]) : Buffer.alloc(0)
  return Buffer.concat([header, query.subarray(12, end), answer])
}

const role = process.argv[2]
// master keeps custom rules first; the studio branch bypasses domestic traffic.
const domesticFirst = role !== '--custom-first'
if (role === '--upstream') {
  const sockets = []
  for (const [port, label] of [[8080, 'direct'], [8081, 'employee-a'], [8082, 'employee-b']]) {
    Bun.serve({ hostname: '0.0.0.0', port, fetch: () => new Response(label) })
    const udp = createSocket('udp4')
    udp.on('message', (_, peer) => udp.send(Buffer.from(label), peer.port, peer.address))
    sockets.push(new Promise(resolve => udp.bind(port, '0.0.0.0', resolve)))
  }
  const dns = createSocket('udp4')
  dns.on('message', (query, peer) => dns.send(dnsAnswer(query, '1.1.1.3'), peer.port, peer.address))
  sockets.push(new Promise(resolve => dns.bind(53, '223.5.5.5', resolve)))
  for (const index of [0, 1]) {
    Bun.serve({
      hostname: '1.1.1.1', port: 8441 + index,
      tls: { cert: Bun.file('/tmp/studio-cert.pem'), key: Bun.file('/tmp/studio-key.pem') },
      async fetch(request) {
        assert.equal(new URL(request.url).pathname, '/dns-query')
        const query = Buffer.from(await request.arrayBuffer())
        return new Response(dnsAnswer(query, `1.1.1.${10 + index}`), {
          headers: { 'Content-Type': 'application/dns-message' },
        })
      },
    })
  }
  await Promise.all(sockets)
  writeFileSync('/tmp/studio-upstream-ready', '')
} else if (role === '--udp' || role === '--dns') {
  const socket = createSocket('udp4')
  const timeout = setTimeout(() => { console.error('studio datagram timeout'); process.exit(1) }, 5000)
  socket.on('error', error => { console.error(error.message); process.exit(1) })
  socket.once('message', message => {
    clearTimeout(timeout)
    if (role === '--dns') {
      assert.equal(message.readUInt16BE(0), 0x1234)
      console.log(JSON.stringify({
        rcode: message[3] & 0x0f,
        answer: message.readUInt16BE(6) ? [...message.subarray(-4)].join('.') : null,
      }))
    } else console.log(message.toString())
    socket.close()
  })
  const packet = role === '--dns' ? Buffer.concat([
    Buffer.from('123401000001000000000000', 'hex'),
    ...process.argv[3].split('.').flatMap(label => [Buffer.from([label.length]), Buffer.from(label)]),
    Buffer.from('0000010001', 'hex'),
  ]) : Buffer.from('studio-udp')
  // Query a non-local resolver through the gateway so the client's IP survives.
  socket.send(packet, role === '--dns' ? 53 : 8080, role === '--dns' ? '8.8.8.8' : process.argv[3])
} else {
  assert.equal(readlinkSync('/proc/self/ns/net'), readlinkSync('/proc/1/ns/net'))
  const namespaces = ['studio-upstream', 'employee-a', 'employee-b']
  const processes = []
  const logs = []
  function command(...args) {
    const result = Bun.spawnSync(args, { stdout: 'pipe', stderr: 'pipe' })
    assert.equal(result.exitCode, 0, `${args.join(' ')}: ${result.stderr}`)
    return result.stdout.toString().trim()
  }
  function spawn(args, name, env = process.env) {
    const path = `/tmp/studio-${name}.log`
    logs.push(path)
    const fd = openSync(path, 'w')
    try {
      const child = Bun.spawn(args, { stdout: fd, stderr: fd, env })
      processes.push(child)
      return child
    } finally { closeSync(fd) }
  }
  async function until(check, message, ms = 15000) {
    const deadline = Date.now() + ms
    while (Date.now() < deadline) {
      if (await check()) return
      await Bun.sleep(50)
    }
    throw new Error(message)
  }
  async function stop(child) {
    if (!child || child.exitCode !== null) return
    child.kill('SIGTERM')
    const exited = await Promise.race([child.exited.then(() => true), Bun.sleep(3000).then(() => false)])
    if (!exited) { child.kill('SIGKILL'); await child.exited }
    assert(exited, 'studio process should exit gracefully')
  }
  async function api(path, body, method) {
    const response = await fetch(`http://127.0.0.1:6161/api/${path}`, {
      method: method || (body === undefined ? 'GET' : 'POST'),
      headers: { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(12000),
    })
    const payload = await response.json()
    assert(response.ok && payload.success, `${path}: ${JSON.stringify(payload)}`)
    return payload.data
  }
  function httpCommand(client, ip, hostname = 'studio.example') {
    return ['ip', 'netns', 'exec', client, 'curl', '--noproxy', '*', '-fsS', '--max-time', '4',
      '--resolve', `${hostname}:8080:${ip}`, `http://${hostname}:8080/`]
  }
  const http = (client, ip, hostname) => command(...httpCommand(client, ip, hostname))
  const udp = (client, ip) => command('ip', 'netns', 'exec', client, '/run/bun', '/run/netns-studio.mjs', '--udp', ip)
  const dns = (client, domain) => JSON.parse(command('ip', 'netns', 'exec', client, '/run/bun', '/run/netns-studio.mjs', '--dns', domain))
  function blockedHttp(client, ip = '1.1.1.2') {
    const result = Bun.spawnSync(httpCommand(client, ip), { stdout: 'pipe', stderr: 'pipe' })
    assert.notEqual(result.exitCode, 0, 'failed pinned node must not fall back to another exit')
  }
  const uuid = '7a965829-1c77-4607-a131-24a523787af3'
  let app
  try {
    command('sysctl', '-qw', 'net.ipv4.ip_forward=1')
    for (const name of namespaces) command('ip', 'netns', 'add', name)
    command('ip', 'link', 'add', 'studio-wan', 'type', 'veth', 'peer', 'name', 'up-peer')
    command('ip', 'link', 'set', 'up-peer', 'netns', 'studio-upstream')
    command('ip', 'address', 'add', '192.0.2.2/24', 'dev', 'studio-wan')
    command('ip', 'link', 'set', 'studio-wan', 'up')
    command('ip', 'route', 'add', 'default', 'via', '192.0.2.1')
    for (const args of [
      ['address', 'add', '192.0.2.1/24', 'dev', 'up-peer'],
      ['link', 'set', 'up-peer', 'up'], ['link', 'set', 'lo', 'up'],
      ['route', 'add', 'default', 'via', '192.0.2.2'],
    ]) command('ip', '-n', 'studio-upstream', ...args)
    for (const address of ['1.1.1.1', '1.1.1.2', '1.1.1.3', '223.5.5.5', '223.5.5.6', '10.10.10.10']) {
      command('ip', '-n', 'studio-upstream', 'address', 'add', `${address}/32`, 'dev', 'lo')
    }
    command('ip', 'link', 'add', 'studio-lan', 'type', 'bridge')
    command('ip', 'address', 'add', '192.168.50.1/24', 'dev', 'studio-lan')
    command('ip', 'link', 'set', 'studio-lan', 'up')
    for (const [index, name] of ['employee-a', 'employee-b'].entries()) {
      command('ip', 'link', 'add', `lan${index}`, 'type', 'veth', 'peer', 'name', `peer${index}`)
      command('ip', 'link', 'set', `peer${index}`, 'netns', name)
      command('ip', 'link', 'set', `lan${index}`, 'master', 'studio-lan')
      command('ip', 'link', 'set', `lan${index}`, 'up')
      for (const args of [
        ['address', 'add', `192.168.50.${101 + index}/24`, 'dev', `peer${index}`],
        ['link', 'set', `peer${index}`, 'up'], ['link', 'set', 'lo', 'up'],
        ['route', 'add', 'default', 'via', '192.168.50.1'],
      ]) command('ip', '-n', name, ...args)
    }
    command('openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-noenc', '-days', '1',
      '-keyout', '/tmp/studio-key.pem', '-out', '/tmp/studio-cert.pem', '-subj', '/CN=1.1.1.1',
      '-addext', 'subjectAltName=IP:1.1.1.1')
    spawn(['ip', 'netns', 'exec', 'studio-upstream', '/run/bun', '/run/netns-studio.mjs', '--upstream'], 'fixtures')
    await until(() => existsSync('/tmp/studio-upstream-ready'), 'studio endpoints did not start')
    for (const index of [0, 1]) {
      writeFileSync(`/tmp/studio-node-${index}.json`, JSON.stringify({
        inbounds: [{ type: 'vless', listen: '192.0.2.1', listen_port: 10001 + index, users: [{ uuid }] }],
        outbounds: [{ type: 'direct', tag: 'direct' }],
        route: { rules: [
          { port: 443, action: 'route', outbound: 'direct', override_port: 8441 + index },
          { port: 8080, action: 'route', outbound: 'direct', override_port: 8081 + index },
        ] },
      }))
    }
    function startNode(index, name) {
      return spawn(['ip', 'netns', 'exec', 'studio-upstream', '/run/sing-box-host', 'run', '-c', `/tmp/studio-node-${index}.json`], name)
    }
    const nodeA = startNode(0, 'node-a')
    startNode(1, 'node-b')
    const nodes = ['employee-a', 'employee-b'].map((tag, index) => ({ type: 'vless', tag, server: '192.0.2.1', server_port: 10001 + index, uuid }))
    writeFileSync('/tmp/studio.yaml', Bun.YAML.stringify({ nodes: nodes.map(node => JSON.stringify(node)), route_mode: 'rule' }))
    async function startApp(name) {
      // Trust only a fixture certificate in this test process tree, not on the host.
      app = spawn(['/run/miao-rust', '--config', '/tmp/studio.yaml'], name, { ...process.env, SSL_CERT_FILE: '/tmp/studio-cert.pem' })
      await until(async () => { try { const status = await api('status'); return status.ready && !status.initializing } catch { return false } }, 'studio gateway did not become ready')
    }
    await startApp('gateway')
    for (const [index, target] of ['employee-a', 'employee-b'].entries()) {
      await api('rules', { field: 'source_ip_cidr', value: `192.168.50.${101 + index}/32`, target })
    }
    for (const [index, client] of ['employee-a', 'employee-b'].entries()) {
      assert.equal(http(client, '1.1.1.2'), client)
      assert.equal(udp(client, '1.1.1.2'), client)
      assert.equal(http(client, '223.5.5.6'), domesticFirst ? 'direct' : client)
      assert.equal(udp(client, '223.5.5.6'), domesticFirst ? 'direct' : client)
      assert.equal(http(client, '1.1.1.2', 'www.baidu.com'), domesticFirst ? 'direct' : client)
      assert.equal(http(client, '10.10.10.10'), domesticFirst ? 'direct' : client)
      assert.deepEqual(dns(client, 'www.baidu.com'), { rcode: 0, answer: domesticFirst ? '1.1.1.3' : `1.1.1.${10 + index}` })
      // Same domain through both clients also catches cross-node DNS cache reuse.
      assert.deepEqual(dns(client, 'overseas.example'), { rcode: 0, answer: `1.1.1.${10 + index}` })
      console.log(`PASS ${client}: pinned TCP/UDP/DNS and expected domestic/private policy`)
    }
    await api('proxy/switch', { group: 'proxy', name: 'employee-b' })
    assert.equal(http('employee-a', '1.1.1.2'), 'employee-a')
    assert.deepEqual(dns('employee-a', 'after-switch.example'), { rcode: 0, answer: '1.1.1.10' })
    console.log('PASS default exit switch leaves employee binding and DNS unchanged')

    await stop(nodeA)
    blockedHttp('employee-a')
    assert.equal(http('employee-b', '1.1.1.2'), 'employee-b')
    if (domesticFirst) assert.equal(http('employee-a', '223.5.5.6'), 'direct')
    else assert.equal(http('employee-b', '223.5.5.6'), 'employee-b')
    console.log('PASS unreachable node A does not fall back or affect B')

    await api('nodes', { tag: 'employee-a' }, 'DELETE')
    blockedHttp('employee-a')
    assert.deepEqual(dns('employee-a', 'overseas.example'), { rcode: 5, answer: null })
    assert.deepEqual(dns('employee-a', 'www.baidu.com'), domesticFirst ? { rcode: 0, answer: '1.1.1.3' } : { rcode: 5, answer: null })
    if (domesticFirst) assert.equal(http('employee-a', '223.5.5.6'), 'direct')
    else assert.equal(http('employee-b', '223.5.5.6'), 'employee-b')
    if (domesticFirst) assert.equal(http('employee-a', '10.10.10.10'), 'direct')
    else blockedHttp('employee-a', '223.5.5.6')
    assert.equal(http('employee-b', '1.1.1.2'), 'employee-b')
    assert.deepEqual(dns('employee-b', 'overseas.example'), { rcode: 0, answer: '1.1.1.11' })
    console.log('PASS missing A blocks its data/DNS; B retains its own DNS cache and exit')

    startNode(0, 'node-a-restored')
    // Deleted tags stay reserved: the add-node API deliberately prevents a new
    // same-name node from taking over dormant bindings. Restore the original
    // local configuration instead, preserving the source and its credentials.
    await stop(app)
    const saved = Bun.YAML.parse(readFileSync('/tmp/studio.yaml', 'utf8'))
    saved.nodes = nodes.map(node => JSON.stringify(node))
    writeFileSync('/tmp/studio.yaml', Bun.YAML.stringify(saved))
    await startApp('gateway-restored')
    assert.equal(http('employee-a', '1.1.1.2'), 'employee-a')
    assert.deepEqual(dns('employee-a', 'overseas.example'), { rcode: 0, answer: '1.1.1.10' })
    console.log('PASS restoring A restores its original data/DNS binding')

    await api('route-mode', { route_mode: 'global' })
    assert.equal(http('employee-a', '223.5.5.6'), 'employee-a')
    assert.deepEqual(dns('employee-a', 'www.qq.com'), { rcode: 0, answer: '1.1.1.10' })
    await api('route-mode', { route_mode: 'rule' })
    assert.equal(http('employee-a', '223.5.5.6'), domesticFirst ? 'direct' : 'employee-a')
    assert.deepEqual(dns('employee-a', 'www.qq.com'), { rcode: 0, answer: domesticFirst ? '1.1.1.3' : '1.1.1.10' })
    console.log('PASS explicit global mode and return to branch routing policy apply to data and DNS')
    console.log(domesticFirst ? 'STUDIO GATEWAY PASS' : 'CUSTOM-FIRST GATEWAY DNS CACHE PASS')
  } catch (error) {
    for (const path of logs) if (existsSync(path)) console.error(`${path}\n${readFileSync(path, 'utf8').split('\n').slice(-35).join('\n')}`)
    throw error
  } finally {
    for (const child of processes.toReversed()) await stop(child)
    for (const name of namespaces) Bun.spawnSync(['ip', 'netns', 'del', name], { stdout: 'ignore', stderr: 'ignore' })
  }
}
