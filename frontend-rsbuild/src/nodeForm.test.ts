import { describe, expect, it } from '@rstest/core'
import { buildNodeRequest } from './nodeForm'
import { EMPTY_NODE_FORM } from './utils'

function form(overrides = {}) {
  return { ...EMPTY_NODE_FORM, ...overrides }
}

describe('buildNodeRequest', () => {
  it('preserves SOCKS5 credentials and excludes stale TLS and transport fields', () => {
    expect(buildNodeRequest('socks', form({ tag: 'socks-node', server: 'proxy.example.com', username: ' user ', password: ' p ', transport_type: 'ws', client_fingerprint: 'chrome' }))).toEqual({
      node_type: 'socks', tag: 'socks-node', server: 'proxy.example.com', server_port: 443, username: ' user ', password: ' p ',
    })
    expect(buildNodeRequest('socks', form({ tag: 'no-auth', server: 'proxy.example.com' }))).not.toHaveProperty('password')
  })

  it('validates SOCKS5 credential pairs and byte lengths', () => {
    for (const fields of [{ username: 'user' }, { password: 'p' }, { username: '中'.repeat(86), password: 'p' }, { username: 'u', password: 'p'.repeat(256) }]) {
      expect(() => buildNodeRequest('socks', form({ tag: 'test', server: 'proxy.example.com', ...fields }))).toThrow(/SOCKS5/)
    }
  })

  it('builds and trims a Hysteria2 request', () => {
    const payload = buildNodeRequest('hysteria2', form({
      tag: '  香港节点  ',
      server: ' node.example.com ',
      password: ' password123 ',
      obfs_type: 'salamander',
      obfs_password: ' obfs-secret ',
    }))

    expect(payload).toMatchObject({
      node_type: 'hysteria2',
      tag: '香港节点',
      server: 'node.example.com',
      server_port: 443,
      password: 'password123',
      obfs_type: 'salamander',
      obfs_password: 'obfs-secret',
    })
  })

  it('keeps only the selected VMess transport fields', () => {
    const payload = buildNodeRequest('vmess', form({
      tag: 'vmess-node',
      server: 'vmess.example.com',
      uuid: '123e4567-e89b-12d3-a456-426614174000',
      transport_type: 'ws',
      transport_path: '/ws',
      transport_host: 'cdn.example.com',
      grpc_service_name: 'stale-service',
    }))

    expect(payload).toMatchObject({
      node_type: 'vmess',
      transport_type: 'ws',
      transport_path: '/ws',
      transport_host: 'cdn.example.com',
    })
    expect(payload).not.toHaveProperty('grpc_service_name')
    expect(payload).not.toHaveProperty('password')
  })

  it('requires a TLS fingerprint for Reality', () => {
    expect(() => buildNodeRequest('vless', form({
      tag: 'reality-node',
      server: 'reality.example.com',
      uuid: '123e4567-e89b-12d3-a456-426614174000',
      reality_public_key: 'public-key',
      client_fingerprint: '',
    }))).toThrow(/TLS 指纹/)
  })

  it('rejects invalid fields before making an API request', () => {
    expect(() => buildNodeRequest('hysteria2', form({
      tag: 'bad/tag',
      server: 'node.example.com',
      password: 'password123',
    }))).toThrow(/节点名称/)
  })
})
