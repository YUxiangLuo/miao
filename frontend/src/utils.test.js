import { describe, expect, it } from 'vitest'
import {
  formatBytes,
  formatDelay,
  formatSpeed,
  formatUptime,
  maskSubscription,
  protocolLabel,
  validateHysteria2Obfs,
  validateNodeTag,
  validatePassword,
  validatePort,
  validateServer,
  validateSubscriptionUrl,
  validateTransport,
  validateUuid,
  validateVlessFlow,
} from './utils.js'

describe('formatters', () => {
  it('formats uptime and throughput values', () => {
    expect(formatUptime(0)).toBe('--')
    expect(formatUptime(65)).toBe('1m 5s')
    expect(formatSpeed(1536)).toBe('1.5 KB/s')
    expect(formatBytes(1048576)).toBe('1.0 MB')
    expect(formatDelay(-1)).toBe('超时')
  })

  it('normalizes protocol labels and subscription display text', () => {
    expect(protocolLabel('ss')).toBe('shadowsocks')
    expect(protocolLabel('vmess')).toBe('vmess')
    expect(protocolLabel('vless')).toBe('vless')
    expect(protocolLabel('trojan')).toBe('trojan')
    expect(protocolLabel('tuic')).toBe('tuic')
    expect(protocolLabel('hysteria2')).toBe('hysteria2')
    expect(maskSubscription('https://example.com/path/to/token123456')).toBe('example.com...en123456')
  })
})

describe('validation', () => {
  it('accepts valid subscription URLs and node fields', () => {
    expect(validateSubscriptionUrl('https://example.com/sub?token=abc')).toBeNull()
    expect(validateNodeTag('香港节点 01')).toBeNull()
    expect(validateServer('node.example.com')).toBeNull()
    expect(validatePort(443)).toBeNull()
    expect(validatePassword('password123')).toBeNull()
    expect(validateUuid('123e4567-e89b-12d3-a456-426614174000')).toBeNull()
    expect(validateTransport('ws', '/path', 'example.com', '')).toBeNull()
    expect(validateVlessFlow('xtls-rprx-vision')).toBeNull()
    expect(validateHysteria2Obfs('salamander', 'obfs-secret')).toBeNull()
  })

  it('rejects invalid subscription URLs and node fields', () => {
    expect(validateSubscriptionUrl('ftp://example.com/sub')).toMatch(/HTTP/)
    expect(validateNodeTag('bad/tag')).toMatch(/只能包含/)
    expect(validateServer('localhost')).toMatch(/点号/)
    expect(validatePort(70000)).toMatch(/范围/)
    expect(validatePassword('short')).toMatch(/太短/)
    expect(validateUuid('not-a-uuid')).toMatch(/UUID/)
    expect(validateTransport('xhttp', '', '', '')).toMatch(/传输层/)
    expect(validateTransport('ws', 'path', '', '')).toMatch(/\//)
    expect(validateVlessFlow('bad-flow')).toMatch(/VLESS/)
    expect(validateHysteria2Obfs('', 'secret')).toMatch(/请先选择/)
  })
})
