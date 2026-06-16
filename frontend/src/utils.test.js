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
  validateOptionalCredentials,
  validatePassword,
  validatePort,
  validateSecret,
  validateServer,
  validateSubscriptionUrl,
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
    expect(protocolLabel('hysteria2')).toBe('hysteria2')
    expect(protocolLabel('socks')).toBe('socks5')
    expect(protocolLabel('trojan')).toBe('trojan')
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
    expect(validateSecret('short')).toBeNull()
    expect(validateOptionalCredentials('', '')).toBeNull()
    expect(validateOptionalCredentials('user', '')).toBeNull()
    expect(validateHysteria2Obfs('salamander', 'obfs-secret')).toBeNull()
  })

  it('rejects invalid subscription URLs and node fields', () => {
    expect(validateSubscriptionUrl('ftp://example.com/sub')).toMatch(/HTTP/)
    expect(validateNodeTag('bad/tag')).toMatch(/只能包含/)
    expect(validateServer('localhost')).toMatch(/点号/)
    expect(validatePort(70000)).toMatch(/范围/)
    expect(validatePassword('short')).toMatch(/太短/)
    expect(validateSecret('')).toMatch(/密码不能为空/)
    expect(validateOptionalCredentials('', 'secret')).toMatch(/用户名/)
    expect(validateHysteria2Obfs('', 'secret')).toMatch(/请先选择/)
  })
})
