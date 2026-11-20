import { describe, expect, it } from '@rstest/core'
import { buildNodeRequest } from '../nodeForm'
import { EMPTY_NODE_FORM } from '../utils'
import { buildTerminalRows } from './terminals'
import { buildAccessPortRows, suggestNextAccessPort } from './accessPorts'
import { buildDedicatedLines } from './lines'
import { matchTradePlatform, summarizeActiveTradePlatforms } from './tradeCatalog'
import { connectionMock, nodeMock, ruleMock } from '../testFixtures'

describe('HarborGate foreign-trade domain models', () => {
  it('merges DHCP terminals, seat routing rules, fail-closed status, and active trade platforms', () => {
    const rows = buildTerminalRows({
      terminals: [
        {
          ip: '192.168.8.180',
          name: '李运营',
          department: '美区组',
          dhcp_hostname: 'YY-PC',
          mac: '80:c5:f2:2b:e3:6c',
        },
        {
          ip: '192.168.8.214',
          dhcp_hostname: 'CAIWU',
          mac: '00:e0:4d:a4:0c:35',
        },
      ],
      rules: [
        ruleMock({
          index: 0,
          field: 'source_ip_cidr',
          value: '192.168.8.180/32',
          target: 'us-static-1',
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
            metadata: { host: 'sellercentral.amazon.com', sourceIP: '192.168.8.180' },
          }),
          uploadSpeed: 1024,
          downloadSpeed: 8192,
        },
        {
          ...connectionMock({
            id: 'c2',
            metadata: { host: 'web.whatsapp.com', sourceIP: '192.168.8.180' },
          }),
          uploadSpeed: 512,
          downloadSpeed: 2048,
        },
      ],
    })

    expect(rows).toHaveLength(2)
    expect(rows[0].ip).toBe('192.168.8.180')
    expect(rows[0].displayName).toBe('李运营')
    expect(rows[0].mode).toBe('dedicated')
    expect(rows[0].target).toBe('us-static-1')
    expect(rows[0].failClosed).toBe(true)
    expect(rows[0].activePlatforms.map((p) => p.id)).toEqual(['amazon', 'whatsapp'])

    expect(rows[1].ip).toBe('192.168.8.214')
    expect(rows[1].displayName).toBe('CAIWU')
    expect(rows[1].mode).toBe('block_overseas')
    expect(rows[1].failClosed).toBe(false)
  })

  it('builds multi-store access port rows with relay chain and anti-detect browser copy formats', () => {
    const rows = buildAccessPortRows({
      gatewayHost: '192.168.8.1',
      accessPorts: [
        {
          id: 'ap-20101',
          name: 'Amazon 美区一店',
          port: 20101,
          outbound: 'us-isp-1',
          username: 'amz1',
          password: 'secret',
          has_auth: true,
          blocked: false,
        },
      ],
      nodes: [
        nodeMock({
          tag: 'us-isp-1',
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
            rule: 'inbound=access-port:ap-20101 => route(us-isp-1)',
            metadata: { host: 'amazon.com', type: 'mixed/access-port:ap-20101' },
          }),
          uploadSpeed: 200,
          downloadSpeed: 4000,
        },
      ],
    })

    expect(rows).toHaveLength(1)
    expect(rows[0].detourRelay).toBe('hk-relay')
    expect(rows[0].connectionCount).toBe(1)
    expect(rows[0].browserPasteText).toBe('192.168.8.1:20101:amz1:secret')
    expect(rows[0].socks5Uri).toBe('socks5://amz1:secret@192.168.8.1:20101')
    expect(rows[0].httpUri).toBe('http://amz1:secret@192.168.8.1:20101')
    expect(suggestNextAccessPort([{ port: 20101 }, { port: 20102 }])).toBe(20103)
  })

  it('enriches dedicated static lines with detour status and bound seat/port counts', () => {
    const lines = buildDedicatedLines({
      nodes: [
        nodeMock({
          tag: 'us-isp-1',
          node_type: 'socks',
          detour: 'hk-relay',
        }),
      ],
      rules: [
        ruleMock({ field: 'source_ip_cidr', value: '192.168.8.180/32', target: 'us-isp-1' }),
      ],
      accessPorts: [
        {
          id: 'ap-20101',
          name: 'Store 1',
          port: 20101,
          outbound: 'us-isp-1',
          has_auth: false,
          blocked: false,
        },
      ],
      availableTargetNames: ['hk-relay'],
      delays: { 'us-isp-1': 142 },
      testingNodes: {},
    })

    expect(lines).toHaveLength(1)
    expect(lines[0].isStaticResidential).toBe(true)
    expect(lines[0].relayExists).toBe(true)
    expect(lines[0].boundSeatCount).toBe(1)
    expect(lines[0].boundPortCount).toBe(1)
    expect(lines[0].delay).toBe(142)
  })

  it('validates and builds residential SOCKS5 NodeRequest with detour', () => {
    const req = buildNodeRequest('socks', {
      ...EMPTY_NODE_FORM,
      tag: 'us-isp-socks',
      server: '198.51.100.25',
      server_port: 1080,
      username: 'u1',
      password: 'p1',
      detour: 'hk-relay-01',
    })
    expect(req).toEqual({
      node_type: 'socks',
      tag: 'us-isp-socks',
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

  it('identifies active foreign-trade platforms from live connections', () => {
    expect(matchTradePlatform('web.whatsapp.com')?.id).toBe('whatsapp')
    expect(matchTradePlatform('work.weixin.qq.com')?.id).toBe('wecom')
    const summary = summarizeActiveTradePlatforms([
      {
        ...connectionMock({ id: '1', chains: ['us-isp-1'], metadata: { host: 'www.linkedin.com' } }),
        uploadSpeed: 100,
        downloadSpeed: 900,
      },
    ])
    expect(summary).toHaveLength(1)
    expect(summary[0].platform.id).toBe('linkedin')
  })
})
