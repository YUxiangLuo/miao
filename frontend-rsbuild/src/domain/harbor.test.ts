import { describe, expect, it } from '@rstest/core'
import { buildNodeRequest } from '../nodeForm'
import { EMPTY_NODE_FORM } from '../utils'
import { buildTerminalRows, terminalModeLabel } from './terminals'
import { buildAccessPortRows, suggestNextAccessPort } from './accessPorts'
import { buildDedicatedLines } from './lines'
import { RULE_PRESETS } from './rulePresets'
import { connectionMock, nodeMock, ruleMock } from '../testFixtures'

describe('Harbor domain models', () => {
  it('merges LAN terminals, source-IP routing rules, live speeds, and fail-closed status', () => {
    const rows = buildTerminalRows({
      terminals: [
        {
          ip: '192.168.8.180',
          name: 'MacBook Pro',
          department: '开发机',
          dhcp_hostname: 'MBP-DEV',
          mac: '80:c5:f2:2b:e3:6c',
        },
        {
          ip: '192.168.8.214',
          dhcp_hostname: 'NAS-01',
          mac: '00:e0:4d:a4:0c:35',
        },
      ],
      rules: [
        ruleMock({
          index: 0,
          field: 'source_ip_cidr',
          value: '192.168.8.180/32',
          target: 'us-node-1',
          skipped: true,
        }),
        ruleMock({
          index: 1,
          field: 'source_ip_cidr',
          value: '192.168.8.214/32',
          target: 'reject',
          skipped: false,
        }),
      ],
      connections: [
        {
          ...connectionMock({
            id: 'c1',
            metadata: { host: 'api.github.com', sourceIP: '192.168.8.180' },
          }),
          uploadSpeed: 1024,
          downloadSpeed: 8192,
        },
        {
          ...connectionMock({
            id: 'c2',
            metadata: { host: 'registry.npmjs.org', sourceIP: '192.168.8.250' },
          }),
          uploadSpeed: 512,
          downloadSpeed: 2048,
        },
      ],
    })

    expect(rows).toHaveLength(3)
    expect(rows[0].ip).toBe('192.168.8.180')
    expect(rows[0].displayName).toBe('MacBook Pro')
    expect(rows[0].mode).toBe('dedicated')
    expect(rows[0].target).toBe('us-node-1')
    expect(rows[0].failClosed).toBe(true)

    // Unregistered active IP sorts ahead of offline devices and falls back cleanly to its raw IPv4 address
    expect(rows[1].ip).toBe('192.168.8.250')
    expect(rows[1].displayName).toBe('192.168.8.250')
    expect(rows[1].mode).toBe('pool')
    expect(terminalModeLabel(rows[1].mode)).toBe('默认代理')

    expect(rows[2].ip).toBe('192.168.8.214')
    expect(rows[2].displayName).toBe('NAS-01')
    expect(rows[2].mode).toBe('block_overseas')
    expect(rows[2].failClosed).toBe(false)
    expect(terminalModeLabel(rows[2].mode)).toBe('禁止走代理')
  })

  it('builds independent proxy port rows with relay chain and standard URI formats', () => {
    const rows = buildAccessPortRows({
      gatewayHost: '192.168.8.1',
      accessPorts: [
        {
          id: 'ap-20101',
          name: '独立端口 01',
          port: 20101,
          outbound: 'us-socks-1',
          username: 'u1',
          password: 'secret',
          has_auth: true,
          blocked: false,
        },
      ],
      nodes: [
        nodeMock({
          tag: 'us-socks-1',
          node_type: 'socks',
          server: '198.51.100.12',
          server_port: 1080,
          detour: 'hk-relay',
        }),
      ],
      connections: [
        {
          ...connectionMock({
            id: 'ap-conn-1',
            rule: 'inbound=access-port:ap-20101 => route(us-socks-1)',
            metadata: { host: 'example.com', type: 'mixed/access-port:ap-20101' },
          }),
          uploadSpeed: 200,
          downloadSpeed: 4000,
        },
      ],
    })

    expect(rows).toHaveLength(1)
    expect(rows[0].detourRelay).toBe('hk-relay')
    expect(rows[0].connectionCount).toBe(1)
    expect(rows[0].browserPasteText).toBe('192.168.8.1:20101:u1:secret')
    expect(rows[0].socks5Uri).toBe('socks5://u1:secret@192.168.8.1:20101')
    expect(rows[0].httpUri).toBe('http://u1:secret@192.168.8.1:20101')
    expect(suggestNextAccessPort([{ port: 20101 }, { port: 20102 }])).toBe(20103)
  })

  it('enriches manual nodes with standard protocol labels, detour status, and bound counts', () => {
    const lines = buildDedicatedLines({
      nodes: [
        nodeMock({
          tag: 'us-socks-1',
          node_type: 'socks',
          detour: 'hk-relay',
        }),
      ],
      rules: [
        ruleMock({ field: 'source_ip_cidr', value: '192.168.8.180/32', target: 'us-socks-1' }),
      ],
      accessPorts: [
        {
          id: 'ap-20101',
          name: 'Port 1',
          port: 20101,
          outbound: 'us-socks-1',
          has_auth: false,
          blocked: false,
        },
      ],
      availableTargetNames: ['hk-relay'],
      delays: { 'us-socks-1': 142 },
      testingNodes: {},
    })

    expect(lines).toHaveLength(1)
    expect(lines[0].isSocks).toBe(true)
    expect(lines[0].protocolLabel).toBe('SOCKS5')
    expect(lines[0].relayExists).toBe(true)
    expect(lines[0].boundSeatCount).toBe(1)
    expect(lines[0].boundPortCount).toBe(1)
    expect(lines[0].delay).toBe(142)
  })

  it('validates and builds SOCKS5 NodeRequest with detour', () => {
    const req = buildNodeRequest('socks', {
      ...EMPTY_NODE_FORM,
      tag: 'us-socks',
      server: '198.51.100.25',
      server_port: 1080,
      username: 'u1',
      password: 'p1',
      detour: 'hk-relay-01',
    })
    expect(req).toEqual({
      node_type: 'socks',
      tag: 'us-socks',
      server: '198.51.100.25',
      server_port: 1080,
      username: 'u1',
      password: 'p1',
      detour: 'hk-relay-01',
    })

    expect(() => buildNodeRequest('socks', {
      ...EMPTY_NODE_FORM,
      tag: 'self-node',
      server: '198.51.100.25',
      server_port: 1080,
      detour: 'self-node',
    })).toThrow('自身')
  })

  it('provides clean general-purpose rule presets', () => {
    expect(RULE_PRESETS.length).toBeGreaterThanOrEqual(5)
    expect(RULE_PRESETS.some((p) => p.id === 'openai' && p.recommendedTarget === 'proxy')).toBe(true)
    expect(RULE_PRESETS.some((p) => p.id === 'block-quic' && p.recommendedTarget === 'reject')).toBe(true)
  })
})
